import { connect, type ControlClient } from "./control";
import { DEFAULT_CONTROL_PORT } from "./defaults";
import { markConnected } from "./auto-reconnect";
import { LocalizedError } from "./localized-error";
import { getStoredToken, isTrustedHost } from "./pairing";
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

export function controlClient(): ControlClient | null {
  return client;
}

export function controlHost(): string {
  return hostAddr;
}

export async function connectHost(host: string, port = DEFAULT_CONTROL_PORT): Promise<ControlClient> {
  if (!isTrustedHost(host)) {
    throw new LocalizedError("trustedHostError");
  }
  const usb = await getUsbState();
  let c: ControlClient;
  if (usb.attached && usb.controlPort > 0) {
    try {
      c = await connect("127.0.0.1", usb.controlPort, 5000, () => getStoredToken());
    } catch {
      c = await connect(host, port, 5000, () => getStoredToken(), secureOptions(host, port));
    }
  } else {
    c = await connect(host, port, 5000, () => getStoredToken(), secureOptions(host, port));
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
    const previous = client;
    const usb = await getUsbState();
    let c: ControlClient;
    if (usb.attached && usb.controlPort > 0) {
      try {
        c = await connect("127.0.0.1", usb.controlPort, 5000, () => getStoredToken());
      } catch {
        c = await connect(hostTarget, hostPort, 5000, () => getStoredToken(), secureOptions(hostTarget, hostPort));
      }
    } else {
      c = await connect(hostTarget, hostPort, 5000, () => getStoredToken(), secureOptions(hostTarget, hostPort));
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

/** Allocate the next viewer-side UDP port for a new stream window. */
export function allocPort(): number {
  return nextPort++;
}

/** Terminate the active control session and clear client state. */
export function disconnectHost(): void {
  if (client) {
    client.close();
    client = null;
  }
  hostAddr = "";
  hostTarget = "";
  reconnectInFlight = null;
}
