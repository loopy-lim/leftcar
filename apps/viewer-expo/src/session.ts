import { connect, type ControlClient } from "./control";
import { getStoredToken } from "./pairing";

/**
 * App-wide control session singleton: the hub screen connects once, catalog
 * and stream management reuse the same client.
 */

let client: ControlClient | null = null;
let hostAddr = "";
let hostTarget = "";
let hostPort = 7777;
let nextPort = 5001;
let reconnectInFlight: Promise<ControlClient> | null = null;

export function controlClient(): ControlClient | null {
  return client;
}

export function controlHost(): string {
  return hostAddr;
}

export async function connectHost(host: string, port = 7777): Promise<ControlClient> {
  const c = await connect(host, port, 5000, () => getStoredToken());
  // Keep the previous connection alive until the replacement succeeds, then
  // release it so switching between multiple computers does not leak sockets.
  if (client && client !== c) client.close();
  client = c;
  hostAddr = `${host}:${port}`;
  hostTarget = host;
  hostPort = port;
  return c;
}

/** Reopen the control socket after the host app was restarted. */
export async function reconnectHost(): Promise<ControlClient> {
  if (!hostTarget) throw new Error("호스트 주소가 없습니다");
  if (reconnectInFlight) return reconnectInFlight;

  reconnectInFlight = (async () => {
    const previous = client;
    const c = await connect(hostTarget, hostPort, 5000, () => getStoredToken());
    if (previous && previous !== c) previous.close();
    client = c;
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
