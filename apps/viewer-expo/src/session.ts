import { connect, type ControlClient } from "./control";
import { DEFAULT_CONTROL_PORT } from "./defaults";
import { markConnected } from "./auto-reconnect";
import { LocalizedError } from "./localized-error";
import { getStoredToken, isTrustedHost } from "./pairing";
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
      c = await connect(host, port, 5000, () => getStoredToken());
    }
  } else {
    c = await connect(host, port, 5000, () => getStoredToken());
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
        c = await connect(hostTarget, hostPort, 5000, () => getStoredToken());
      }
    } else {
      c = await connect(hostTarget, hostPort, 5000, () => getStoredToken());
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
