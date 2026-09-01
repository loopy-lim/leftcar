import { describe, expect, it, vi } from "vitest";
import type { ControlClient } from "./control";
import {
  reconfigurePreparedStream,
  startPreparedStream,
  type StartStreamArgs,
  type StreamLauncher,
} from "./launch-stream";

const args: StartStreamArgs = {
  sourceIndex: 0,
  viewerPort: 5010,
  width: 3840,
  height: 2160,
  fps: 60,
  captureBackend: "screenCaptureKit",
  mediaTransport: "udp",
  encoderExperiment: "auto",
  displayName: "Main",
};

function launcher(): StreamLauncher {
  return {
    prepareStream: vi.fn(async () => undefined),
    openStream: vi.fn(async () => "src-5010"),
    cancelPreparedStream: vi.fn(async () => undefined),
  };
}

describe("adaptive stream receipts", () => {
  it("opens the dimensions accepted by Host", async () => {
    const native = launcher();
    const control: ControlClient = {
      request: vi.fn(async (command: string) => {
        if (command === "startStream") {
          return { session: 42, width: 3840, height: 2160, fps: 60, qualityState: "native" };
        }
        throw new Error(`unexpected command ${command}`);
      }) as ControlClient["request"],
      close: vi.fn(),
    };
    const started = await startPreparedStream({
      control,
      launcher: native,
      host: "192.168.0.134",
      advertisedEncoderExperiments: [{
        id: "auto",
        label: "자동",
        hint: "자동",
        requiresReconnect: true,
      }],
      args,
    });
    expect(started).toMatchObject({
      session: 42,
      width: 3840,
      height: 2160,
      fps: 60,
      qualityState: "native",
    });
    expect(native.openStream).toHaveBeenCalledWith(
      5010,
      "192.168.0.134",
      3840,
      2160,
      60,
      "auto",
      "Main",
      true,
    );
  });

  it("reconfigures the same session and port, then opens the accepted fallback", async () => {
    const native = launcher();
    const control: ControlClient = {
      request: vi.fn(async (command: string) => {
        if (command === "reconfigureStream") {
          return { session: 42, width: 2560, height: 1440, fps: 60, qualityState: "fallback" };
        }
        throw new Error(`unexpected command ${command}`);
      }) as ControlClient["request"],
      close: vi.fn(),
    };
    const result = await reconfigurePreparedStream({
      control,
      launcher: native,
      host: "192.168.0.134",
      active: {
        ...args,
        port: args.viewerPort,
        session: 42,
        sourceName: "Main",
        viewerIps: ["192.168.0.18"],
        mediaTransport: "udp",
        encoderExperiment: "auto",
        captureBackend: args.captureBackend,
        contentMode: "interactive",
        startedAt: Date.now(),
        sourceTarget: { width: args.width, height: args.height, fps: args.fps },
        activeTarget: { width: args.width, height: args.height, fps: args.fps },
        fallbackTarget: { width: 2560, height: 1440, fps: args.fps },
        qualityState: "native",
      },
      target: { width: 2560, height: 1440, fps: 60 },
      qualityState: "fallback",
    });
    expect(result).toMatchObject({ session: 42, width: 2560, height: 1440, qualityState: "fallback" });
    expect(control.request).toHaveBeenCalledWith("reconfigureStream", {
      session: 42,
      width: 2560,
      height: 1440,
      fps: 60,
      qualityState: "fallback",
    });
    expect(native.openStream).toHaveBeenCalledWith(
      5010,
      "192.168.0.134",
      2560,
      1440,
      60,
      "auto",
      "Main",
      true,
    );
  });
});
