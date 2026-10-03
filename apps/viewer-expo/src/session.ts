import { connect, type ControlClient } from "./control";
import { DEFAULT_CONTROL_PORT } from "./defaults";
import { markConnected } from "./auto-reconnect";
import { LocalizedError } from "./localized-error";
import {
  getStoredCredential,
  isTrustedHost,
  type HostEndpoint,
  type StoredCredential,
} from "./pairing";
import { getStoredPinnedHostKey, registerPinnedHostKey, rememberPinnedHostKey } from "./pinned-host-keys";
import { getUsbState } from "./usb";

/**
 * App-wide control session singleton: the hub screen connects once, catalog
 * and stream management reuse the same client.
 */

let client: ControlClient | null = null;
let hostAddr = "";
let hostTarget = "";
let hostPort = DEFAULT_CONTROL_PORT;
let nextPort = 5001;
let reconnectInFlight: { generation: number; promise: Promise<ControlClient> } | null = null;
/**
 * 연결 시도 세대. disconnect나 새 connectHost가 세대를 올리고, 늦게 끝나는
 * 이전 시도는 자기 세대가 최신이 아니면 소켓을 닫고 물러난다 — 대기 중이던
 * 연결이 이미 끊긴(또는 다른 호스트로 바뀐) 세션을 되살리지 않는다.
 */
let connectGeneration = 0;
let selectionCancellation = new AbortController();
let activeContext: SessionRequestContext | null = null;
const errorContexts = new WeakMap<object, SessionRequestContext>();

export interface HostSelection {
  readonly generation: number;
  readonly signal: AbortSignal;
}

export interface SessionRequestContext {
  readonly client: ControlClient;
  readonly target: HostEndpoint;
  readonly selectionGeneration: number;
  /** Verified network identity; null for the endpoint-bound USB path. */
  readonly identity: string | null;
  /** Exact token incarnation cached by this socket. */
  readonly credential: StoredCredential | null;
}

export interface ConnectHostOptions {
  selection?: HostSelection;
  signal?: AbortSignal;
}

function abortError(): Error {
  const error = new Error("host selection superseded");
  error.name = "AbortError";
  return error;
}

function assertNotAborted(signal?: AbortSignal): void {
  if (signal?.aborted) throw abortError();
}

/** Bind pending socket/storage work to both the user selection and its screen. */
function connectionCancellation(selectionSignal: AbortSignal, actionSignal?: AbortSignal) {
  const controller = new AbortController();
  const sources = actionSignal ? [selectionSignal, actionSignal] : [selectionSignal];
  const cancel = () => controller.abort();
  sources.forEach((source: AbortSignal) => {
    if (source.aborted) cancel();
    else source.addEventListener("abort", cancel, { once: true });
  });
  return {
    signal: controller.signal,
    dispose() {
      sources.forEach((source: AbortSignal) => source.removeEventListener("abort", cancel));
    },
  };
}

function assertSelectionCurrent(selection: HostSelection, signal?: AbortSignal): void {
  if (signal?.aborted || selection.generation !== connectGeneration) throw abortError();
}

export function isHostSelectionCurrent(selection: HostSelection): boolean {
  return selection.generation === connectGeneration;
}

/** Reserve a user-selection generation before any async discovery/storage work. */
export function beginHostSelection(): HostSelection {
  selectionCancellation.abort();
  selectionCancellation = new AbortController();
  return { generation: ++connectGeneration, signal: selectionCancellation.signal };
}

export function controlClient(): ControlClient | null {
  return client;
}

/** 연결 상태가 바뀔 때(성립·끊김·해제) 발화한다. 홈 배지가 이 통지로 진실을
 * 유지한다 — 죽은 연결을 "연결됨"으로 표시하지 않게 하는 계기. 반환값은
 * 구독 해제 함수다. */
const connectionListeners = new Set<() => void>();

export function subscribeConnectionChanged(listener: () => void): () => void {
  connectionListeners.add(listener);
  return () => connectionListeners.delete(listener);
}

function notifyConnectionChanged(): void {
  for (const listener of [...connectionListeners]) listener();
}

export function controlHost(): string {
  return hostAddr;
}

/** 현재 제어 세션의 대상 엔드포인트(세션이 없으면 null) — 401 시 그 대상의 토큰만 치운다. */
export function controlTarget(): HostEndpoint | null {
  return hostTarget ? { host: hostTarget, port: hostPort } : null;
}

/** Capture client, target, selection, and credential at the request's async origin. */
export function captureRequestContext(): SessionRequestContext | null {
  return activeContext;
}

export function isRequestContextCurrent(context: SessionRequestContext): boolean {
  return activeContext === context &&
    (client === context.client || client === null) &&
    connectGeneration === context.selectionGeneration;
}

function watchControlClose(socket: ControlClient): void {
  socket.whenClosed(() => {
    if (client !== socket) return;
    client = null;
    hostAddr = "";
    // Transport loss is not a user selection change. Native windows retain
    // the verified target/selection so they can reconnect without MainActivity.
    // Explicit disconnect and new selections still invalidate this context.
    notifyConnectionChanged();
  });
}

export function bindRequestContext(error: unknown, context: SessionRequestContext): void {
  if ((typeof error === "object" && error !== null) || typeof error === "function") {
    errorContexts.set(error as object, context);
  }
}

export function requestContextForError(error: unknown): SessionRequestContext | null {
  if ((typeof error !== "object" || error === null) && typeof error !== "function") return null;
  return errorContexts.get(error as object) ?? null;
}

/**
 * 제어 소켓 하나를 연다. USB 액세서리가 붙어 있으면 루프백 포트를 먼저
 * 시도하고(호스트 앱 재시작에도 살아 있는 경로), 실패하면 네트워크로 폴백한다.
 * 두 경로 모두 이 대상 호스트의 토큰을 쓴다 — 액세서리는 대상의 프록시다.
 */
async function openControl(
  target: string,
  port: number,
  signal?: AbortSignal,
): Promise<{ client: ControlClient; credential: StoredCredential | null; identity: string | null }> {
  const endpoint = { host: target, port };
  const usb = await getUsbState();
  assertNotAborted(signal);
  if (usb.attached && usb.controlPort > 0) {
    try {
      const credential = await getStoredCredential(endpoint, null, signal);
      const usbClient = await connect(
        "127.0.0.1",
        usb.controlPort,
        5000,
        async () => credential?.token ?? null,
      );
      return { client: usbClient, credential, identity: null };
    } catch {
      assertNotAborted(signal);
      // 루프백 실패는 네트워크 경로로 이어 시도한다.
    }
  }
  assertNotAborted(signal);
  const pinnedHostKey = await getStoredPinnedHostKey(target, port);
  assertNotAborted(signal);
  const credentialOptions = {
    allowEndpointCredential: pinnedHostKey !== null || target === "127.0.0.1" || target === "localhost",
  };
  let verifiedHostKey: string | null = null;
  let credentialPromise: Promise<StoredCredential | null> | null = null;
  const networkClient = await connect(
    target,
    port,
    5000,
    async () => {
      credentialPromise ??= getStoredCredential(endpoint, verifiedHostKey, signal, credentialOptions);
      return (await credentialPromise)?.token ?? null;
    },
    secureOptions(
      target,
      port,
      pinnedHostKey,
      (key) => {
        verifiedHostKey = key;
      },
      signal,
    ),
  );
  try {
    if (signal?.aborted) throw abortError();
    verifiedHostKey = networkClient.hostKey ?? verifiedHostKey;
    const credential = await getStoredCredential(endpoint, verifiedHostKey, signal, credentialOptions);
    // Credential migration owns the endpoint/identity/recent-host transaction.
    // Only an unpaired connection needs this standalone TOFU persistence; doing
    // it before migration would leave the alias committed if token migration
    // later rolled back.
    if (verifiedHostKey && !credential) {
      await rememberPinnedHostKey(target, port, verifiedHostKey, signal);
    }
    credentialPromise = Promise.resolve(credential);
    return { client: networkClient, credential, identity: verifiedHostKey };
  } catch (error) {
    networkClient.close();
    throw error;
  }
}

export async function connectHost(
  host: string,
  port = DEFAULT_CONTROL_PORT,
  options: ConnectHostOptions = {},
): Promise<ControlClient> {
  if (!isTrustedHost(host)) {
    throw new LocalizedError("trustedHostError");
  }
  const selection = options.selection ?? beginHostSelection();
  assertSelectionCurrent(selection, options.signal);
  const cancellation = connectionCancellation(selection.signal, options.signal);
  const signal = cancellation.signal;
  try {
    const opened = await openControl(host, port, signal);
    const c = opened.client;
    try {
      assertSelectionCurrent(selection, signal);
    } catch (error) {
      // 대기 중에 disconnect나 더 새로운 connectHost가 이겼다 — 늦게 도착한
      // 이 소켓은 닫고 상태는 그대로 둔다.
      c.close();
      throw error;
    }
    // Keep the previous connection alive until the replacement succeeds, then
    // release it so switching between multiple computers does not leak sockets.
    const previous = client;
    client = c;
    hostAddr = `${host}:${port}`;
    hostTarget = host;
    hostPort = port;
    activeContext = {
      client: c,
      target: { host, port },
      selectionGeneration: selection.generation,
      identity: opened.identity,
      credential: opened.credential,
    };
    if (previous && previous !== c) previous.close();
    markConnected();
    // 소켓이 저절로 닫히면(호스트 재시작·네트워크 전환) 죽은 클라이언트를
    // "연결됨"으로 표시하지 않고 자동 재연결이 동작하도록 상태를 즉시
    // 무효화한다. 우리가 close()한 경우엔 disconnectHost가 이미 정리했으므로
    // 이 클라이언트가 아닐 때는 무시한다.
    watchControlClose(c);
    notifyConnectionChanged();
    return c;
  } finally {
    cancellation.dispose();
  }
}

/**
 * 후보 주소를 순서대로 시도해 처음 열리는 제어 소켓을 반환한다. 같은 호스트의
 * LAN 대표 주소와 테일넷 별칭을 하나의 엔트리로 들고 다니기 때문에, 집·밖
 * 어디서든 호출부는 후보 목록만 넘기면 된다. 취소(선택 교체·화면 종료)는
 * 즉시 위로 던지고, 순수 네트워크 실패만 다음 후보로 넘어간다. 성공 시 실제로
 * 연결된 주소를 돌려준다 — 저장·표시가 이 주소 기준으로 정렬된다.
 */
export async function connectHostWithFallback(
  hosts: readonly string[],
  port = DEFAULT_CONTROL_PORT,
  options: ConnectHostOptions = {},
): Promise<string> {
  let lastError: unknown = new LocalizedError("trustedHostError");
  for (const host of hosts) {
    try {
      await connectHost(host, port, options);
      return host;
    } catch (error) {
      // 사용자가 이미 다른 곳으로 떠났다면 남은 후보를 시도하지 않는다.
      if (options.signal?.aborted) throw error;
      if (options.selection && !isHostSelectionCurrent(options.selection)) throw error;
      lastError = error;
    }
  }
  throw lastError;
}

/** A caller owns only its wait, while the selected Host owns the shared socket. */
function waitForReconnect(promise: Promise<ControlClient>, signal?: AbortSignal): Promise<ControlClient> {
  if (!signal) return promise;
  return new Promise((resolve, reject) => {
    const cancel = () => reject(abortError());
    if (signal.aborted) { cancel(); return; }
    signal.addEventListener("abort", cancel, { once: true });
    promise.then(resolve, reject).finally(() => signal.removeEventListener("abort", cancel));
  });
}

/** Reopen the control socket after the host app was restarted. */
export async function reconnectHost(
  origin: SessionRequestContext | null = captureRequestContext(),
  signal?: AbortSignal,
): Promise<ControlClient> {
  assertNotAborted(signal);
  if (!origin) throw new LocalizedError("errNoReconnectTarget");
  if (!isRequestContextCurrent(origin)) throw abortError();
  if (reconnectInFlight?.generation === origin.selectionGeneration) {
    return waitForReconnect(reconnectInFlight.promise, signal);
  }

  const promise = (async () => {
    const cancellation = connectionCancellation(selectionCancellation.signal);
    try {
      const previous = origin.client;
      const opened = await openControl(origin.target.host, origin.target.port, cancellation.signal);
      const c = opened.client;
      if (cancellation.signal.aborted || !isRequestContextCurrent(origin)) {
        c.close();
        throw abortError();
      }
      client = c;
      hostAddr = `${origin.target.host}:${origin.target.port}`;
      activeContext = {
        client: c,
        target: origin.target,
        selectionGeneration: origin.selectionGeneration,
        identity: opened.identity,
        credential: opened.credential,
      };
      if (previous && previous !== c) previous.close();
      watchControlClose(c);
      markConnected();
      notifyConnectionChanged();
      return c;
    } finally {
      cancellation.dispose();
    }
  })();
  reconnectInFlight = { generation: origin.selectionGeneration, promise };
  // Retirement belongs to the attempt, never an individual cancelled waiter.
  const retire = () => {
    if (reconnectInFlight?.promise === promise) reconnectInFlight = null;
  };
  void promise.then(retire, retire);
  return waitForReconnect(promise, signal);
}

/**
 * 핸드셰이크 핀 옵션. 저장된 핀이 있으면 대조(불일치 시 연결 거부), 없으면
 * TOFU — 검증된 키를 메모리와 recent hosts에 핀한다.
 */
function secureOptions(
  host: string,
  port: number,
  pinnedHostKey: string | null,
  onVerified: (key: string) => void,
  signal?: AbortSignal,
): { pinnedHostKey: string | null; onHostKey: (key: string) => void } {
  return {
    pinnedHostKey,
    onHostKey: (key) => {
      if (signal?.aborted) return;
      onVerified(key);
      registerPinnedHostKey(host, port, key);
    },
  };
}

/**
 * Allocate `count` consecutive viewer-side UDP ports for a new stream window.
 *
 * Streams reserve their full wire footprint up front: a splitVertical stream
 * occupies its base port AND base+1 (`prepared_udp.rs split_ports` — the host
 * sends the right tile to base+1), and a later adaptive reconfigure can also
 * promote an existing single-mode stream at the same base to split. Reserving
 * two ports per window keeps the next openDisplay from receiving the split's
 * right-tile port, which used to hard-fail the "4K split + second display"
 * scenario with a bind EADDRINUSE (ERR_STREAM_PREPARE).
 */
export function allocPorts(count: number): number {
  const base = nextPort;
  nextPort += Math.max(1, Math.floor(count));
  return base;
}

/** Allocate the next viewer-side UDP port for a single-socket stream. */
export function allocPort(): number {
  return allocPorts(1);
}

/** Terminate the active control session and clear client state. */
export function disconnectHost(context?: SessionRequestContext): boolean {
  if (context && !isRequestContextCurrent(context)) return false;
  // 대기 중인 connect/reconnect를 무효화한다 — 늦게 끝나는 시도가 상태를
  // 되살리지 못하게 세대를 올린다.
  selectionCancellation.abort();
  connectGeneration += 1;
  if (client) {
    client.close();
    client = null;
  }
  hostAddr = "";
  hostTarget = "";
  activeContext = null;
  reconnectInFlight = null;
  notifyConnectionChanged();
  return true;
}
