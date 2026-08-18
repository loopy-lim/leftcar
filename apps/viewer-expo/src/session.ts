import { connect, type ControlClient } from "./control";

/**
 * App-wide control session singleton: the hub screen connects once, catalog
 * and stream management reuse the same client.
 */

let client: ControlClient | null = null;
let hostAddr = "";
let nextPort = 5001;

export function controlClient(): ControlClient | null {
  return client;
}

export function controlHost(): string {
  return hostAddr;
}

export async function connectHost(host: string, port = 7777): Promise<ControlClient> {
  const c = await connect(host, port);
  client = c;
  hostAddr = `${host}:${port}`;
  return c;
}

export function disconnectHost(): void {
  client?.close();
  client = null;
  hostAddr = "";
}

/** Allocate the next viewer-side TCP port for a new stream window. */
export function allocPort(): number {
  return nextPort++;
}
