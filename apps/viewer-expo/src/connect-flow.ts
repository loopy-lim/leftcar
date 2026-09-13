import { Alert } from "react-native";
import { router } from "expo-router";
import { markPairingStale } from "./auto-reconnect";
import { currentTranslation } from "./language-store";
import {
  clearStoredCredential,
  clearToken,
  createPairingAttempt,
  pairWithHost,
  pairWithHostApproval,
  pairWithHostByCode,
  type HostEndpoint,
  type PairingAttempt,
  type QrPayload,
} from "./pairing";
import {
  captureRequestContext,
  connectHost,
  controlTarget,
  disconnectHost,
  isRequestContextCurrent,
  type SessionRequestContext,
} from "./session";

/**
 * 401(토큰 만료) 공통 반응: 토큰 폐기 → 연결 해제 → 선택적 부가 처리 →
 * 안내 창 → 페어링 화면. 조용히 끝낼지(자동 재연결), 어떤 주소를 페어링에
 * 넘길지는 호출부가 고른다. 폐기는 토큰 발급자(현재 제어 대상)의 키만 지운다
 * — 다른 페어링된 호스트는 잠기지 않는다.
 */
export async function handleUnauthorized(options: {
  /** 연결 해제 직후, 안내·이동 전에 끝낼 화면 상태 갱신. */
  beforeNavigate?: () => void;
  /** 재연결 게이트를 페어링 만료로 막는다 — 조용한 자동 재연결 경로용. */
  markStale?: boolean;
  navigate?: { endpoint?: string; replace?: boolean };
  /** Context captured before the request that returned 401. */
  context?: SessionRequestContext | null;
  /** Cancel UI publication when the originating screen loses focus. */
  signal?: AbortSignal;
} = {}): Promise<void> {
  // Explicit null means the error arrived without a provable request origin.
  // Resolving a mutable global target here could retire a newer host.
  if (options.context === null) return;
  const context = options.context === undefined ? captureRequestContext() : options.context;
  if (context?.credential) await clearStoredCredential(context.credential);
  else if (!context) {
    // Compatibility for a genuinely context-free direct caller. A known
    // tokenless context has no credential incarnation to retire.
    const target = controlTarget();
    if (target) await clearToken(target);
  }
  // A late 401 still retires only its own credential incarnation, but it must
  // not tear down or navigate away from a newer user selection.
  if (options.signal?.aborted || (context && !isRequestContextCurrent(context))) return;
  disconnectHost(context ?? undefined);
  if (options.markStale) markPairingStale();
  options.beforeNavigate?.();
  if (options.navigate) {
    Alert.alert(
      currentTranslation().viewer.pairingRequiredTitle,
      currentTranslation().viewer.pairingRequiredDesc,
    );
    const { endpoint, replace } = options.navigate;
    if (replace) {
      router.replace({ pathname: "/pairing", params: { endpoint } });
    } else {
      router.push({ pathname: "/pairing", params: { endpoint } });
    }
  }
}

export interface PairingWorkflowRun {
  readonly key: string;
  readonly attempt: PairingAttempt;
  /** Predecessor rollback must finish before this run can mutate Host/storage. */
  readonly ready: Promise<void>;
}

/**
 * Own the screen's current pairing lifetime. Repeated camera callbacks for the
 * same approval offer reuse the live run; a different QR or any new PIN aborts
 * the predecessor before its next side-effect boundary.
 */
export class PairingWorkflow {
  private current: PairingWorkflowRun | null = null;
  private nextPinRun = 0;

  beginQr(payload: QrPayload): PairingWorkflowRun | null {
    const key = `qr:${payload.id}:${payload.secret}:${payload.hostKey}`;
    if (this.current?.key === key) return null;
    return this.replace(key);
  }

  beginPin(): PairingWorkflowRun {
    this.nextPinRun += 1;
    return this.replace(`pin:${this.nextPinRun}`);
  }

  finish(run: PairingWorkflowRun): boolean {
    if (this.current !== run) return false;
    this.current = null;
    return true;
  }

  async cancel(): Promise<void> {
    const run = this.current;
    this.current = null;
    if (run) await run.attempt.cancel();
  }

  private replace(key: string): PairingWorkflowRun {
    const previous = this.current;
    const run = {
      key,
      attempt: createPairingAttempt(),
      ready: previous?.attempt.cancel() ?? Promise.resolve(),
    };
    this.current = run;
    return run;
  }
}

export async function runQrPairingWorkflow(options: {
  run: PairingWorkflowRun;
  payload: QrPayload;
  onPending?: () => void;
  navigate: () => void;
}): Promise<void> {
  await options.run.ready;
  let result: Awaited<ReturnType<typeof pairWithHostApproval>> | null = null;
  await finishPairingAttempt({
    attempt: options.run.attempt,
    pair: async () => {
      result = await pairWithHostApproval(options.payload, {
        attempt: options.run.attempt,
        onPending: options.onPending,
      });
      if (result.kind !== "approved") throw new Error("pairing rejected");
    },
    connect: async () => {
      await connectHost(options.payload.host, options.payload.port, {
        signal: options.run.attempt.signal,
      });
    },
    navigate: options.navigate,
  });
}

export async function runPinPairingWorkflow(options: {
  run: PairingWorkflowRun;
  target: QrPayload | HostEndpoint;
  code: string;
  navigate: () => void;
}): Promise<void> {
  await options.run.ready;
  await finishPairingAttempt({
    attempt: options.run.attempt,
    pair: async () => {
      if ("id" in options.target) {
        await pairWithHost(options.target, options.code, { attempt: options.run.attempt });
      } else {
        await pairWithHostByCode(options.target.host, options.target.port, options.code, {
          attempt: options.run.attempt,
        });
      }
    },
    connect: async () => {
      await connectHost(options.target.host, options.target.port, {
        signal: options.run.attempt.signal,
      });
    },
    navigate: options.navigate,
  });
}

/**
 * Keep pairing persistence, authenticated connection, and navigation inside
 * one cancelable lifetime. Any failure or cancellation rolls back credentials
 * written by this attempt unless a newer incarnation has already replaced them.
 */
export async function finishPairingAttempt(options: {
  attempt: PairingAttempt;
  pair: () => Promise<void>;
  connect: () => Promise<void>;
  navigate: () => void;
}): Promise<void> {
  try {
    options.attempt.throwIfCancelled();
    await options.pair();
    options.attempt.throwIfCancelled();
    await options.connect();
    options.attempt.throwIfCancelled();
    options.navigate();
    options.attempt.commit();
  } catch (error) {
    await options.attempt.cancel();
    throw error;
  }
}
