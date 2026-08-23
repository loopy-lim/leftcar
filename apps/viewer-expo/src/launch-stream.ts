import type { ControlClient } from "./control";

export interface StreamLauncher {
  getLocalIpv4Addresses?(): Promise<string[]>;
  prepareStream(port: number, host: string): Promise<void>;
  openStream(
    port: number,
    host: string,
    width: number,
    height: number,
    fps: number,
  ): Promise<string>;
  cancelPreparedStream(port: number): Promise<void>;
}

export interface StartStreamArgs {
  sourceIndex: number;
  viewerPort: number;
  width: number;
  height: number;
  fps: number;
  captureBackend: string;
  viewerIps?: string[];
}

export interface StartedStream {
  session: number;
  viewerIps: string[];
}

export type StreamControlRequest = <T>(command: string, args?: unknown) => Promise<T>;

interface StartPreparedStreamInput {
  control: ControlClient;
  request?: StreamControlRequest;
  launcher: StreamLauncher;
  host: string;
  args: StartStreamArgs;
}

/**
 * Bind the viewer port before Host reachability proof, then display the native
 * window only after the Host has created a real capture session. A failure at
 * either boundary rolls both sides back instead of leaving a black document.
 */
export async function startPreparedStream({
  control,
  request = control.request.bind(control),
  launcher,
  host,
  args,
}: StartPreparedStreamInput): Promise<StartedStream> {
  let session: number | null = null;
  try {
    const discoveredIps = launcher.getLocalIpv4Addresses
      ? await launcher.getLocalIpv4Addresses().catch(() => [])
      : [];
    const viewerIps = [...new Set(discoveredIps)]
      .filter((address) => typeof address === "string" && address.length > 0)
      .slice(0, 4);
    const startArgs = viewerIps.length > 0 ? { ...args, viewerIps } : args;
    await launcher.prepareStream(args.viewerPort, host);
    const started = await request<{ session: number }>("startStream", startArgs);
    session = started.session;
    await launcher.openStream(
      args.viewerPort,
      host,
      args.width,
      args.height,
      args.fps,
    );
    return { session, viewerIps };
  } catch (error) {
    if (session !== null) {
      await request("stopStream", { session }).catch(() => undefined);
    }
    await launcher.cancelPreparedStream(args.viewerPort).catch(() => undefined);
    throw error;
  }
}
