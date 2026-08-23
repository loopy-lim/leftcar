import type { ControlClient } from "./control";

export interface StreamLauncher {
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
}

interface StartPreparedStreamInput {
  control: ControlClient;
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
  launcher,
  host,
  args,
}: StartPreparedStreamInput): Promise<number> {
  let session: number | null = null;
  try {
    await launcher.prepareStream(args.viewerPort, host);
    const started = await control.request<{ session: number }>("startStream", args);
    session = started.session;
    await launcher.openStream(
      args.viewerPort,
      host,
      args.width,
      args.height,
      args.fps,
    );
    return session;
  } catch (error) {
    if (session !== null) {
      await control.request("stopStream", { session }).catch(() => undefined);
    }
    await launcher.cancelPreparedStream(args.viewerPort).catch(() => undefined);
    throw error;
  }
}
