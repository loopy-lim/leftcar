import { connect, type ControlClient } from "./control";
import { DEFAULT_CONTROL_PORT } from "./defaults";
import { markConnected } from "./auto-reconnect";
import { LocalizedError } from "./localized-error";
import { getStoredToken, isTrustedHost, type HostEndpoint } from "./pairing";
import { getPinnedHostKey, registerPinnedHostKey, rememberPinnedHostKey } from "./pinned-host-keys";
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
let reconnectInFlight: Promise<ControlClient> | null = null;
/**
 * 연결 시도 세대. disconnect나 새 connectHost가 세대를 올리고, 늦게 끝나는
 * 이전 시도는 자기 세대가 최신이 아니면 소켓을 닫고 물러난다 — 대기 중이던
 * 연결이 이미 끊긴(또는 다른 호스트로 바뀐) 세션을 되살리지 않는다.
 */
let connectGeneration = 0;

export function controlClient(): ControlClient | null {
  return client;
}

export function controlHost(): string {
  return hostAddr;
}

/** 현재 제어 세션의 대상 엔드포인트(세션이 없으면 null) — 401 시 그 대상의 토큰만 치운다. */
export function controlTarget(): HostEndpoint | null {
  return hostTarget ? { host: hostTarget, port: hostPort } : null;
}

/**
 * 제어 소켓 하나를 연다. USB 액세서리가 붙어 있으면 루프백 포트를 먼저
 * 시도하고(호스트 앱 재시작에도 살아 있는 경로), 실패하면 네트워크로 폴백한다.
 * 두 경로 모두 이 대상 호스트의 토큰을 쓴다 — 액세서리는 대상의 프록시다.
 */
async function openControl(target: string, port: number): Promise<ControlClient> {
  const usb = await getUsbState();
  if (usb.attached && usb.controlPort > 0) {
    try {
      return await connect("127.0.0.1", usb.controlPort, 5000, () =>
        getStoredToken({ host: target, port }),
      );
    } catch {
      // 루프백 실패는 네트워크 경로로 이어 시도한다.
    }
  }
  return connect(
    target,
    port,
    5000,
    () => getStoredToken({ host: target, port }),
    secureOptions(target, port),
  );
}

export async function connectHost(host: string, port = DEFAULT_CONTROL_PORT): Promise<ControlClient> {
  if (!isTrustedHost(host)) {
    throw new LocalizedError("trustedHostError");
  }
  const generation = ++connectGeneration;
  const c = await openControl(host, port);
  if (generation !== connectGeneration) {
    // 대기 중에 disconnect나 더 새로운 connectHost가 이겼다 — 늦게 도착한
    // 이 소켓은 닫고 상태는 그대로 둔다.
    c.close();
    throw new LocalizedError("errGeneric");
  }
  // Keep the previous connection alive until the replacement succeeds, then
  // release it so switching between multiple computers does not leak sockets.
  if (client && client !== c) client.close();
  client = c;
  hostAddr = `${host}:${port}`;
  hostTarget = host;
  hostPort = port;
  markConnected();
  return c;
}

/** Reopen the control socket after the host app was restarted. */
export async function reconnectHost(): Promise<ControlClient> {
  if (!hostTarget) throw new LocalizedError("errNoReconnectTarget");
  if (reconnectInFlight) return reconnectInFlight;

  reconnectInFlight = (async () => {
    const generation = ++connectGeneration;
    const previous = client;
    const c = await openControl(hostTarget, hostPort);
    if (generation !== connectGeneration) {
      c.close();
      throw new LocalizedError("errGeneric");
    }
    if (previous && previous !== c) previous.close();
    client = c;
    markConnected();
    return c;
  })();
  try {
    return await reconnectInFlight;
  } finally {
    reconnectInFlight = null;
  }
}

/**
 * 핸드셰이크 핀 옵션. 저장된 핀이 있으면 대조(불일치 시 연결 거부), 없으면
 * TOFU — 검증된 키를 메모리와 recent hosts에 핀한다.
 */
function secureOptions(
  host: string,
  port: number,
): { pinnedHostKey: string | null; onHostKey: (key: string) => void } {
  const pinned = getPinnedHostKey(host, port);
  return {
    pinnedHostKey: pinned,
    onHostKey: (key) => {
      if (pinned) return;
      rememberPinnedHostKey(host, port, key);
    },
  };
}

/** 앱 시작 시 recent hosts의 핀을 메모리로 복원한다. */
export async function restorePinnedHostKeys(): Promise<void> {
  const { getRecentHosts } = await import("./recent-hosts");
  const hosts = await getRecentHosts();
  for (const item of hosts) {
    if (item.hostKey) registerPinnedHostKey(item.host, item.port, item.hostKey);
  }
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
export function disconnectHost(): void {
  // 대기 중인 connect/reconnect를 무효화한다 — 늦게 끝나는 시도가 상태를
  // 되살리지 못하게 세대를 올린다.
  connectGeneration += 1;
  if (client) {
    client.close();
    client = null;
  }
  hostAddr = "";
  hostTarget = "";
  reconnectInFlight = null;
}
